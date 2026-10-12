<script lang="ts">
  /* My profile (route `profile`, #/me): the active account's profile. Picture, name, status, about, availability and
     additional information; each piece saves on its own and patches one key. Accepted state is in the store; drafts
     live in the rows. Other people's profiles are read-only (PersonProfile). */
  import { untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';
  import './profile.css';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  import FieldRow from './FieldRow.svelte';
  import Availability from './Availability.svelte';
  import PictureFlow from './PictureFlow.svelte';
  import AddInfo from './AddInfo.svelte';
  import OpenLink from './OpenLink.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { px, profileOf } from '../../lib/model/actions/profile.svelte';
  import { removePicture, setField, setName, LIMITS, type Result } from '../../lib/model/actions/profile';
  import { inspectImage, type Picked } from '../../lib/model/images';
  import { KNOWN, extraKeys, labelOf, normaliseLink } from './info';
  import { FIELD_HELP, PICTURE_HELP } from './copy';

  const prof = $derived(profileOf('me'));
  const fields = $derived(prof?.fields ?? {});
  const extras = $derived(extraKeys(fields));
  const hasPic = $derived(!!prof?.picture);
  const offline = $derived(!db.env.networkOnline);
  const hasOther = $derived(extras.some((k) => !(k in KNOWN)));

  let picked = $state<Extract<Picked, { ok: true }> | null>(null);
  let picError = $state('');
  let removeAsk = $state(false);
  let removeError = $state('');
  let addOpen = $state(false);
  let linkUrl = $state<string | null>(null);
  let announce = $state('');
  let fileEl = $state<HTMLInputElement>();
  let opener: HTMLElement | null = null;
  const pickedUrl = $derived(picked?.url);
  const fadeMs = $derived(motion.reduced ? 0 : dur('crossfade'));
  const layerOpen = $derived(!!picked || removeAsk || addOpen || !!linkUrl);

  function open<T>(set: (v: T) => void, v: T) { opener = document.activeElement as HTMLElement; set(v); }
  function closeLayers() { picked = null; removeAsk = false; addOpen = false; linkUrl = null; queueMicrotask(() => opener?.isConnected && opener.focus({ preventScroll: true })); }

  /* ---- picture ---- */
  /** The picker and the drawer both end up here. A rejected file changes nothing; the message says why. */
  async function choose(file: Blob) {
    picError = '';
    const r = await inspectImage(file);
    if (!r.ok) { picError = r.error; return; }
    picked = r;
  }
  function picker(e: Event & { currentTarget: HTMLInputElement }) {
    const f = e.currentTarget.files?.[0];
    e.currentTarget.value = '';            // so choosing the same file again still counts
    if (f) void choose(f);                 // cancelling gives no file: nothing happens
  }
  $effect(() => {
    const o = px.offered;
    if (o) untrack(() => { px.offered = null; void choose(o.file); });
  });
  function pictureDone() { picked = null; announce = 'Picture updated'; }
  function remove() {
    const r = removePicture();
    if (r.ok) { removeAsk = false; removeError = ''; announce = 'Picture removed'; } else removeError = r.error;
  }

  /* ---- field saves ---- */
  const optionalText = (key: string) => (v: string): Result => setField(key, v.trim() === '' ? undefined : v.trim());
  const saveLink = (key: string) => (v: string): Result => {
    if (v.trim() === '') return setField(key, undefined);
    const u = normaliseLink(v);
    return u ? setField(key, u) : { ok: false, error: 'Enter a web address that starts with http:// or https://.' };
  };
  const drop = (key: string) => () => setField(key, undefined);
</script>

<div class="pf" data-profile="me">
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>My profile</b></header>

  <div class="pf-scroll" inert={layerOpen}>
  <div class="pf-col">
    <section class="hero" aria-label="Picture">
      <Avatar name={db.account.name} hue={db.account.hue} person="me" size={112} />
      <div class="btns">
        {#if hasPic}
          <Btn variant="outline" onclick={() => fileEl?.click()}><Icon name="camera" size={18} />Change photo</Btn>
          <Btn variant="outline" onclick={(e: MouseEvent) => { opener = e.currentTarget as HTMLElement; removeError = ''; removeAsk = true; }}>Remove</Btn>
        {:else}
          <Btn variant="outline" onclick={() => fileEl?.click()}><Icon name="camera" size={18} />Add photo</Btn>
        {/if}
      </div>
      <input bind:this={fileEl} type="file" class="vh" tabindex="-1" accept="image/png,image/jpeg,image/webp" aria-label="Choose a picture file" onchange={picker} data-file />
    </section>

    <section class="pf-sec" aria-labelledby="p-h">
      <h2 id="p-h">Profile</h2>
      <div class="pf-card">
        <FieldRow field="name" label="Name" value={db.account.name} limit={LIMITS.name} {offline} placeholder="Your name" onsave={setName} />
        <FieldRow field="status" label="Status" value={fields.status} optional limit={LIMITS.text} {offline} empty="Add a status" placeholder="What are you up to?" onsave={optionalText('status')} onremove={drop('status')} />
        <FieldRow field="about" label="About" value={fields.about} kind="multiline" optional limit={LIMITS.text} {offline} empty="Add a few words about you" onsave={optionalText('about')} onremove={drop('about')} />
      </div>
    </section>

    <section class="pf-sec" aria-labelledby="a-h">
      <h2 id="a-h">Availability</h2>
      <div class="pf-card"><Availability {offline} /></div>
      <p class="pf-note">Your contacts can see this, and it’s the same on all your devices. It affects whether calls reach you, and it’s separate from whether you’re online.</p>
    </section>

    <section class="pf-sec" aria-labelledby="i-h">
      <h2 id="i-h">Additional information</h2>
      <div class="pf-card" data-extra>
        {#each extras as k (k)}
          {@const known = KNOWN[k]}
          <FieldRow field={k} label={labelOf(k)} value={fields[k]} kind={known?.kind ?? 'text'} hint={known?.hint} optional {offline} limit={LIMITS.text}
            onsave={known?.kind === 'link' ? saveLink(k) : optionalText(k)} onremove={drop(k)} onopen={(u) => open((v) => (linkUrl = v), u)} />
        {/each}
        <button type="button" class="pf-act press" onclick={() => open((v) => (addOpen = v), true)}><Icon name="plus" size={20} /><span class="t"><b>Add information</b></span></button>
      </div>
      <p class="pf-note">{FIELD_HELP}</p>
      {#if hasOther}<p class="pf-note">Some information came from another device or app version. It’s shown as stored and kept when you edit anything else.</p>{/if}
    </section>
  </div>
  </div>

  <p class="vh" role="status">{announce}</p>

  <!-- Floating, not in the page flow: these appear on their own and must not move the page. -->
  {#if picError}
    <div class="pf-err pf-float perr" role="alert" data-pic-error transition:fade={{ duration: fadeMs }}><span>{picError}</span><button type="button" class="x press" aria-label="Dismiss" onclick={() => (picError = '')}><Icon name="x" size={18} /></button></div>
  {:else if px.pending > 0}
    <div class="pf-strip pf-float" role="status" data-pending transition:fade={{ duration: fadeMs }}><Icon name="clock" size={16} />Saved on this device. Will sync when you’re connected.</div>
  {/if}

  {#if picked}
    {#key pickedUrl}<PictureFlow {picked} replacing={hasPic} onclose={closeLayers} ondone={pictureDone} />{/key}
  {:else if removeAsk}
    <Layer variant="dialog" label="Remove your picture" onclose={closeLayers}>
      <div class="pf-dlg" data-remove-picture>
        <h2>Remove your picture?</h2>
        <p class="lead">Your initials will show instead.</p>
        <p>{PICTURE_HELP}</p>
        {#if removeError}<p class="pf-err" role="alert">{removeError}</p>{/if}
        <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={closeLayers}>Cancel</Btn><Btn variant="danger" onclick={remove}>Remove picture</Btn></div>
      </div>
    </Layer>
  {:else if addOpen}
    <AddInfo have={Object.keys(fields)} onclose={closeLayers} />
  {:else if linkUrl}
    <OpenLink url={linkUrl} onclose={closeLayers} />
  {/if}
</div>

<style>
  .hero { display: grid; justify-items: center; padding: 24px 20px 4px; }
  /* Equal-width pair so the two buttons sit centred under the picture (a quiet text button would pull the pair off-centre). */
  .btns { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(140px, auto); justify-content: center; gap: 8px; margin-block-start: 18px; }
  .pf-scroll { padding-block-end: 96px; }   /* room for the floating notes at the end of the page */
  .perr { display: flex; align-items: center; gap: 8px; margin-block-start: 0; padding-block: 4px; padding-inline: 12px 4px; }
  .perr span { flex: 1; min-inline-size: 0; }
  .perr .x { flex: none; inline-size: var(--btn); block-size: var(--btn); border: 0; border-radius: 50%; background: none; color: inherit; display: grid; place-items: center; cursor: pointer; }
  .perr .x:hover { background: color-mix(in srgb, currentColor 12%, transparent); }
</style>
