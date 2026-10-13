<script lang="ts">
  /* Person profile (route `contact`): someone else's account profile, read-only, then a visibly separate block of
     settings that are only mine (nickname, calls, mute, block). Viewing a profile never adds anyone as a contact,
     and a person with no accessible profile gets the plain fallback. */
  import './profile.css';
  import { fade } from 'svelte/transition';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  import FieldRow from './FieldRow.svelte';
  import OpenLink from './OpenLink.svelte';
  import { db, firstName, person } from '../../lib/model/store.svelte';
  import { clock } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { call } from '../../lib/calls/call.svelte';
  import { effectiveAvailability, ensureChat, setAlias } from '../../lib/model/actions/profile';
  import { profileOf } from '../../lib/model/actions/profile.svelte';
  import { KNOWN, extraKeys, labelOf, untilLabel } from './info';

  let { personId }: { personId: string } = $props();

  const p = $derived(person(personId));
  const prof = $derived(profileOf(personId));
  const fields = $derived(prof?.fields ?? {});
  const extras = $derived(extraKeys(fields));
  const fn = $derived(firstName(personId));
  const status = $derived(typeof fields.status === 'string' ? fields.status : '');
  const about = $derived(typeof fields.about === 'string' ? fields.about : '');
  const fadeMs = $derived(motion.reduced ? 0 : dur('crossfade'));
  const av = $derived(effectiveAvailability(personId, clock.now));
  const avText = $derived(av.until === undefined ? 'Busy' : `Busy until ${untilLabel(av.until)}`);
  const chat = $derived(db.chats.find((c) => c.kind === 'direct' && c.person === personId));
  const refusal = $derived(call.refusal(personId));
  const callOk = $derived(call.canCall(personId));
  const callWhy = $derived(refusal === 'blocked' ? 'You blocked them' : refusal === 'messages-only' ? `Messages only: calling needs ${fn}’s permission` : '');

  let blockAsk = $state(false);
  let linkUrl = $state<string | null>(null);
  let opener: HTMLElement | null = null;
  const layerOpen = $derived(blockAsk || !!linkUrl);
  function ask(set: () => void, e: Event) { opener = e.currentTarget as HTMLElement; set(); }
  function closeLayers() { blockAsk = false; linkUrl = null; queueMicrotask(() => opener?.isConnected && opener.focus({ preventScroll: true })); }

  /** Back into the chat we came from, or open it (a first message creates the chat; merely viewing never did). */
  function message() {
    if (!p?.contact || p.blocked) return;
    const c = ensureChat(personId);
    if (!c) return;
    const prev = nav.stack[nav.stack.length - 2];
    if (prev?.name === 'conversation' && prev.chat === c.id) nav.back();
    else if (prefs.form === 'desktop') nav.open({ name: 'conversation', chat: c.id });
    else nav.push({ name: 'conversation', chat: c.id });
  }
  function block() { if (p) p.blocked = true; closeLayers(); }
</script>

<div class="pf" data-profile={personId}>
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>Profile</b></header>

  <div class="pf-scroll" inert={layerOpen}>
  <div class="pf-col">
  {#if !p}
    <p class="pf-none">Person not found.</p>
  {:else}
    <section class="hero">
      <Avatar name={p.name} hue={p.hue} person={p.id} size={112} />
      <h1 dir="auto" data-their-name>{p.name}</h1>
      {#if status}<p class="status" dir="auto">{status}</p>{/if}
      <!-- The Busy chip has a row of its own, empty when they are available, so it coming and going moves nothing. -->
      {#if prof}<div class="avslot">{#if !av.available}<p class="avl busy" data-availability transition:fade={{ duration: fadeMs }}>{avText}<span class="vh">. A setting they chose, not whether they’re online.</span></p>{/if}</div>{/if}
      {#if p.contact}
        <div class="btns">
          <Btn variant="primary" disabled={p.blocked} onclick={message}><Icon name="chat" size={18} />Message</Btn>
          <Btn variant="outline" disabled={!callOk} title={callWhy} onclick={() => call.place(personId)}><Icon name="phone" size={18} />Call</Btn>
        </div>
        <p class="why" aria-live="polite">{#if callWhy}<span transition:fade={{ duration: fadeMs }}>{callWhy}</span>{/if}</p>
      {/if}
    </section>

    {#if p.blocked}
      <div class="pf-strip" role="status"><Icon name="off" size={16} />You blocked {fn}. Their profile no longer updates.</div>
    {/if}

    {#if !prof}
      <p class="pf-note fallback" data-no-profile>{#if p.contact}{fn}’s profile hasn’t arrived yet. It shows here once it syncs.{:else}No profile is available.{/if}</p>
    {:else}
      {#if about}
        <section class="pf-sec" aria-labelledby="ab-h">
          <h2 id="ab-h">About</h2>
          <div class="pf-card"><FieldRow field="about" label="" value={about} kind="multiline" readonly /></div>
        </section>
      {/if}
      {#if extras.length}
        <section class="pf-sec" aria-labelledby="in-h">
          <h2 id="in-h">Additional information</h2>
          <div class="pf-card" data-extra>
            {#each extras as k (k)}
              <FieldRow field={k} label={labelOf(k)} value={fields[k]} kind={KNOWN[k]?.kind ?? 'text'} readonly onopen={(u) => (linkUrl = u)} />
            {/each}
          </div>
        </section>
      {/if}
    {/if}

    {#if p.contact}
    <section class="pf-sec mine" aria-labelledby="mine-h" data-mine>
      <h2 id="mine-h">Your settings for {fn}</h2>
        <div class="pf-card">
          <FieldRow field="alias" label="Nickname" value={p.alias} optional empty="Add a nickname" placeholder={p.name}
            note={p.alias ? `Shown to you as ${p.alias} · their name is ${p.name}` : ''} onsave={(v) => setAlias(personId, v)} onremove={() => setAlias(personId, '')} />
          <label class="pf-act row-sw"><Icon name="phone" size={20} /><span class="t"><b>Allow calls</b><small>Off means messages only</small></span>
            <Switch label="Allow calls" bind:checked={p.callsAllowed} /></label>
          {#if chat}
            <label class="pf-act row-sw"><Icon name="mute" size={20} /><span class="t"><b>Mute notifications</b><small>Messages still arrive; you just aren’t alerted</small></span>
              <Switch label="Mute notifications" bind:checked={chat.muted} /></label>
          {/if}
          {#if p.blocked}
            <button type="button" class="pf-act press" onclick={() => (p.blocked = false)}><Icon name="off" size={20} /><span class="t"><b>Unblock {fn}</b></span></button>
          {:else}
            <button type="button" class="pf-act press danger" onclick={(e) => ask(() => (blockAsk = true), e)}><Icon name="off" size={20} /><span class="t"><b>Block {fn}</b></span></button>
          {/if}
        </div>
        <p class="pf-note">Only you see these. A nickname changes what you call {fn}, not their profile.</p>
    </section>
    {:else}
      <p class="pf-note solo" data-not-contact>{fn} isn’t one of your contacts. You know them through a group. Looking at a profile doesn’t add anyone.</p>
    {/if}
  {/if}
  </div>
  </div>

  {#if blockAsk && p}
    <Layer variant="dialog" label="Block {fn}" onclose={closeLayers}>
      <div class="pf-dlg" data-block>
        <h2>Block {fn}?</h2>
        <p class="lead">You won’t get calls or messages from {fn}, and their profile updates stop.</p>
        <p>Groups you share aren’t affected. You can unblock {fn} here at any time.</p>
        <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={closeLayers}>Cancel</Btn><Btn variant="danger" onclick={block}>Block</Btn></div>
      </div>
    </Layer>
  {:else if linkUrl}
    <OpenLink url={linkUrl} onclose={closeLayers} />
  {/if}
</div>

<style>
  .hero { display: grid; justify-items: center; text-align: center; padding: 24px 20px 4px; }
  h1 { margin: 16px 0 0; font-size: 22px; line-height: 28px; font-weight: 650; overflow-wrap: anywhere; }
  .status { margin: 4px 0 0; color: var(--muted); line-height: 21px; overflow-wrap: anywhere; max-inline-size: 36ch; text-wrap: balance; }
  .avslot { justify-self: stretch; display: grid; place-items: center; min-block-size: 26px; margin-block-start: 6px; }
  .avl { margin: 0; padding: 3px 12px; border-radius: 13px; background: var(--surface-2); color: var(--muted); font-size: var(--fs-sub); line-height: 20px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .avl.busy { background: var(--warn-bg); color: var(--warn-ink); }
  .btns { display: flex; gap: 8px; margin-block-start: 14px; }
  .why { margin: 8px 0 0; min-block-size: 19px; display: grid; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }   /* the line is reserved: flipping Allow calls moves nothing */
  .fallback { text-align: center; margin: 20px 24px 0; }
  /* The block that belongs to me stands apart from their profile: more room above, and a rule. */
  .mine { position: relative; margin-block-start: 36px; padding-block-start: 22px; }
  .mine::before { content: ''; position: absolute; inset-block-start: 0; inset-inline: 16px; border-block-start: 1px solid var(--hairline); }
  .solo { text-align: center; margin: 20px 24px 0; }
  .row-sw { cursor: default; }
</style>
