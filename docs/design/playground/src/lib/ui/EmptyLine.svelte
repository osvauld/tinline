<script lang="ts">
  /* The one empty state. Every "nothing here yet" in the app is this, in one of two sizes:
     - hero: a new account's first screen (Android Chats and Calls tabs, the desktop detail pane). The mark's line is
       slack while you have nobody; when your first contact arrives it pulls tight in two tugs (LINE_SPRING, the same
       pull as sign-up Done) and the copy turns from "ready" to "open". The `line` cue is NOT played here: adding a
       contact already plays it, so sound and tug land together.
     - quiet: an empty list in an app you already use (no calls yet, no groups yet). A small still mark and one line.
     All copy lives here so one function decides it; screens only say where they are. */
  import Mark from '../brand/Mark.svelte';
  import Btn from '../../screens/groups/Btn.svelte';
  import { LINE_SPRING } from '../brand/springs';
  import { db, firstName } from '../model/store.svelte';
  import { open as openDialog, pp } from '../model/actions/people.svelte';
  import { ensureChat } from '../model/actions/profile';
  import { nav } from '../nav/router.svelte';
  import { prefs } from '../model/prefs.svelte';

  type Where = 'chats' | 'calls' | 'contacts' | 'groups' | 'pane' | 'unread';
  let { where, size = 'hero' }: { where: Where; size?: 'hero' | 'quiet' } = $props();

  const contacts = $derived(db.people.filter((p) => p.contact && !p.blocked));
  const one = $derived(contacts.length === 1 ? contacts[0] : undefined);
  const phone = $derived(prefs.form === 'android');

  const hero = $derived(
    !contacts.length && pp.sent.length
      ? { title: 'Your line is ready', line: `Your request to ${pp.sent[0].name.split(' ')[0]} is waiting. Your line opens when they accept.`, act: 'Add another contact' }
      : !contacts.length
      ? { title: 'Your line is ready', line: 'Add someone you’d like to talk to, and you can message and call each other.', act: 'Add my first contact' }
      : one
        ? { title: 'Your line is open', line: `Say hello to ${firstName(one.id)} whenever you like.`, act: `Message ${firstName(one.id)}` }
        : { title: 'Your line is open', line: 'Pick someone and say hello whenever you like.', act: 'Start a chat' },
  );
  const quiet = $derived(({
    chats: 'Your chats show up here.',
    unread: 'You’re all caught up.',
    calls: 'Calls you make and receive show up here.',
    contacts: 'The people you add show up here.',
    groups: phone ? 'When someone invites you to a group, it shows up here.' : 'Groups you start or join show up here.',
    pane: 'Your chats show up here.',
  } as Record<Where, string>)[where]);

  function act() {
    // Opens over this screen (no tab switch): closing the sheet reveals the line, now open.
    if (!contacts.length) { openDialog('addContact'); return; }
    if (one) {
      const c = ensureChat(one.id); if (!c) return;
      if (prefs.form === 'desktop') nav.open({ name: 'conversation', chat: c.id }); else nav.push({ name: 'conversation', chat: c.id });
      return;
    }
    openDialog('newChat');
  }
</script>

{#if size === 'hero'}
  <div class="hero" data-empty={where}>
    <div class="art"><Mark size={88} live lineColor state={contacts.length ? 'taut' : 'slack'} spring={LINE_SPRING} label="Tinline" /></div>
    <h3>{hero.title}</h3>
    <p>{hero.line}</p>
    <Btn variant="primary" data-empty-act onclick={act}>{hero.act}</Btn>
  </div>
{:else}
  <div class="quiet" data-empty={where}>
    <Mark size={40} dim />
    <p>{quiet}</p>
  </div>
{/if}

<style>
  /* Fixed slots: the title is one line, the line reserves two, the button keeps its width (min 220, as the sign-up
     footer), so "ready" → "open" changes words without moving anything. */
  .hero { block-size: 100%; min-block-size: 360px; display: grid; place-content: center; justify-items: center; text-align: center;
    padding: 24px; gap: 0; color: var(--muted); }
  .art { color: var(--ink); margin-block-end: 18px; }
  .hero h3 { margin: 0 0 6px; color: var(--ink); font-size: var(--fs-title, 20px); line-height: 26px; font-weight: 600; }
  .hero p { margin: 0 0 20px; max-inline-size: 300px; min-block-size: 38px; font-size: var(--fs-sub); line-height: 19px; text-wrap: pretty; }
  .hero :global(.b) { min-inline-size: 220px; }
  .quiet { display: grid; justify-items: center; gap: 10px; margin: 40px auto 0; padding: 0 24px; max-inline-size: 300px; text-align: center;
    color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .quiet p { margin: 0; }
</style>
