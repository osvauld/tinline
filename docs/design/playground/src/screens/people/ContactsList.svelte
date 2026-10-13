<script lang="ts">
  /* Contacts tab: a filter, "Add contact", people waiting for my answer, then my contacts A to Z, then people I blocked.
     Rows are the shared PersonRow; tapping one opens the existing Person profile (route `contact`). */
  import { slide } from 'svelte/transition';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import PersonRow from './PersonRow.svelte';
  import { db, displayName } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { pp, open, answerRequest } from '../../lib/model/actions/people.svelte';
  import { profileOf } from '../../lib/model/actions/profile.svelte';
  import type { Person } from '../../lib/model/types';

  let q = $state('');
  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
  const norm = (s: string) => s.toLocaleLowerCase();
  const match = (p: Person) => !q.trim() || norm(displayName(p.id)).includes(norm(q.trim())) || norm(p.name).includes(norm(q.trim()));
  const sorted = (l: Person[]) => [...l].sort((a, b) => displayName(a.id).localeCompare(displayName(b.id)));
  const mine = $derived(sorted(db.people.filter((p) => p.contact && !p.blocked && match(p))));
  const blocked = $derived(sorted(db.people.filter((p) => p.blocked && match(p))));
  const total = $derived(db.people.filter((p) => p.contact && !p.blocked).length);
  const groups = $derived.by(() => {
    const out: { letter: string; list: Person[] }[] = [];
    for (const p of mine) {
      const l = (displayName(p.id)[0] ?? '#').toLocaleUpperCase();
      const g = out[out.length - 1];
      if (g && g.letter === l) g.list.push(p); else out.push({ letter: l, list: [p] });
    }
    return out;
  });
  const status = (p: Person) => { const s = profileOf(p.id)?.fields.status; return typeof s === 'string' ? s : ''; };
  const sub = (p: Person) => (p.blocked ? 'Blocked' : status(p));
  const go = (p: Person) => (prefs.form === 'desktop' ? nav.open({ name: 'contact', person: p.id }) : nav.push({ name: 'contact', person: p.id }));
  const sel = (p: Person) => nav.top.name === 'contact' && nav.top.person === p.id;
</script>

<div class="wrap" data-contacts>
  <label class="find">
    <Icon name="search" size={18} />
    <input type="search" bind:value={q} placeholder="Find a contact" aria-label="Find a contact" autocomplete="off" />
  </label>

  <ul class="list" role="list">
    <li><button type="button" class="add press" data-add-contact onclick={() => open('addContact')}>
      <span class="plus"><Icon name="userplus" size={22} /></span>
      <span class="t"><b>Add contact</b><small>Scan a code or paste an invite</small></span>
    </button></li>
  </ul>

  {#if pp.requests.length}
    <h2 class="sec" id="rq-h">Waiting for you</h2>
    <ul class="cards" role="list" aria-labelledby="rq-h">
      {#each pp.requests as r (r.id)}
        <li class="card" data-request={r.who} transition:slide={{ duration: ms }}>
          <div class="who"><Avatar name={r.name} hue={r.hue} person={r.who} size={44} />
            <span class="t"><b>{r.name}</b><small>Scanned your code and wants to message you</small></span></div>
          <div class="acts">
            <Btn variant="primary" onclick={() => answerRequest(r.id, true)}>Add {r.name.split(' ')[0]}</Btn>
            <Btn variant="quiet" onclick={() => answerRequest(r.id, false)}>Not now</Btn>
          </div>
          <p class="hint">Adding lets {r.name.split(' ')[0]} message you. You choose later whether to allow calls.</p>
        </li>
      {/each}
    </ul>
  {/if}

  {#each groups as g (g.letter)}
    <h2 class="sec">{g.letter}</h2>
    <ul class="list" role="list">
      {#each g.list as p (p.id)}
        <li><PersonRow {p} sub={sub(p)} selected={sel(p)} onclick={() => go(p)}>
          {#snippet end()}{#if pp.how[p.id] === 'code'}<span class="tag" title="Added from a pasted code. You haven’t confirmed in person that it is them.">Not confirmed</span>{/if}{/snippet}
        </PersonRow></li>
      {/each}
    </ul>
  {/each}

  {#if blocked.length}
    <h2 class="sec" data-blocked-h>Blocked</h2>
    <ul class="list" role="list">
      {#each blocked as p (p.id)}
        <li><PersonRow {p} sub={sub(p)} dim selected={sel(p)} onclick={() => go(p)} /></li>
      {/each}
    </ul>
  {/if}

  <p class="foot" aria-live="polite">
    {#if q.trim() && !mine.length && !blocked.length}No one called “{q.trim()}” in your contacts.
    {:else}{total} {total === 1 ? 'contact' : 'contacts'}{/if}
  </p>
</div>

<style>
  .wrap { padding-block-end: 24px; }
  .find { display: flex; align-items: center; gap: 8px; margin: 4px 16px 8px; padding: 0 14px; block-size: 44px; border-radius: 22px; background: var(--surface-2); color: var(--muted); }
  .find input { flex: 1; min-inline-size: 0; border: 0; background: none; outline: 0; font: inherit; color: var(--ink); }
  .find input::placeholder { color: var(--muted); }
  .find:focus-within { box-shadow: 0 0 0 2px var(--accent); }
  .list, .cards { list-style: none; margin: 0; padding: 0 8px; }
  .add { display: flex; gap: 12px; align-items: center; inline-size: 100%; min-block-size: 64px; padding: 0 8px; border: 0; border-radius: var(--r-ctl); background: transparent; text-align: start; cursor: pointer; color: inherit; font: inherit; }
  .add:hover { background: var(--surface-2); }
  .plus { inline-size: 44px; block-size: 44px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .t { display: grid; gap: 1px; min-inline-size: 0; flex: 1; }
  .t b { font-weight: 600; line-height: 21px; }
  .t small { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .sec { margin: 0; padding: 18px 16px 6px; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .card { margin: 6px 0; padding: 14px; border-radius: var(--r-ctl); background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); display: grid; gap: 10px; }
  .who { display: flex; gap: 12px; align-items: center; }
  .acts { display: flex; gap: 8px; }
  .hint { margin: 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .tag { padding: 2px 8px; border-radius: 10px; background: var(--surface-2); color: var(--muted); font-size: 12px; line-height: 16px; font-weight: 600; white-space: nowrap; }
  .foot { margin: 14px 16px 0; min-block-size: 19px; text-align: center; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
</style>
