/* Messaging during a call (Android). Decided 2026-10-14 in the in-call lab (`?lab=incall`, its middle phone):
   a Message button on the call screen; ← goes back to where I was; in their chat the phone turns green ("Return to
   call"); the strip sits at the bottom, has no End, and slims to a thin line while I type (I may be writing to someone
   else, so it stays). Without `?ic=` the app uses that; the lab passes `?ic=` (a comma list) to try the others.

     msg      a Message button in the call screen's top bar: minimise and open their chat, from anywhere
     back     ← on the call screen: 'where' = the screen I came from (Android back); 'chat' = always their chat
     noend    the strip has no End (it only returns to the call), so nothing red sits under Send
     kbdhide  the strip hides while the keyboard is up (the header button and the notification still lead back)
     kbdslim  the strip stays while the keyboard is up, slimmed to a thin line (name and time only)
     header   in their chat, the header's phone turns green: "Return to call"
     kbd      draw a simulated keyboard when the composer has focus (the playground has none otherwise) */
import { call } from './call.svelte';
import { nav } from '../nav/router.svelte';
import { prefs } from '../model/prefs.svelte';

const ic = new URLSearchParams(location.search).get('ic');
const flags = new Set((ic ?? 'msg,noend,kbdslim,header').split(',').filter(Boolean));

class InCall {
  msg = $state(flags.has('msg'));
  back = $state<'where' | 'chat'>(flags.has('back-chat') ? 'chat' : 'where');
  stripEnd = $state(!flags.has('noend'));
  /** The strip while I type: stays as it is, slims to a thin line, or hides. */
  typing = $state<'keep' | 'slim' | 'hide'>(flags.has('kbdhide') ? 'hide' : flags.has('kbdslim') ? 'slim' : 'keep');
  header = $state(flags.has('header'));
  simKbd = $state(flags.has('kbd'));
  /** The composer has focus (Android form): on a phone the keyboard is up. */
  kbdOpen = $state(false);

  /** Their chat is the screen on top. */
  inTheirChat() {
    const t = nav.top;
    return !!call.peer && t.name === 'conversation' && t.chat === call.peer;
  }
  /** Leave the call screen for their chat (direct chats use the person's id). */
  openTheirChat() {
    if (!call.peer) return;
    if (!this.inTheirChat()) nav.push({ name: 'conversation', chat: call.peer });
  }
  /** ← on the call screen (and Escape). */
  goBack() {
    call.minimize();
    // Focus follows the call to its strip (otherwise it drops to the page when the call screen leaves).
    setTimeout(() => document.querySelector<HTMLElement>('[data-callstrip] .open')?.focus(), 0);
    if (this.back === 'chat' && prefs.form === 'android') this.openTheirChat();
  }
  /** The Message button. */
  message() {
    call.minimize();
    this.openTheirChat();
  }
  /** The strip is hidden while I type (only when that option is on). */
  get stripHidden() { return this.typing === 'hide' && this.kbdOpen; }
  get stripSlim() { return this.typing === 'slim' && this.kbdOpen; }
}
export const incall = new InCall();

if (typeof document !== 'undefined') {
  // Phones only: a computer has no on-screen keyboard to make room for.
  const isComposer = (t: EventTarget | null) => prefs.form === 'android' && t instanceof HTMLElement && !!t.closest('.composer') && t.matches('textarea');
  document.addEventListener('focusin', (e) => { if (isComposer(e.target)) incall.kbdOpen = true; });
  // Focus leaving the whole window (a click in the lab around the phone) is not the keyboard closing: only a move
  // inside the app, or the lab's "Close keyboard", closes it.
  document.addEventListener('focusout', (e) => {
    if (isComposer(e.target)) setTimeout(() => { if (document.hasFocus() && !isComposer(document.activeElement)) incall.kbdOpen = false; });
  });
}

/** The lab drives each phone with scenes (postMessage from the lab page). */
export function startInCallLab() {
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  addEventListener('message', async (e) => {
    if (e.origin !== location.origin || e.data?.t !== 'incall') return;
    const who = 'arjun';
    switch (e.data.what as string) {
      case 'reset':
        if (call.phase !== 'idle') call.hangup();
        (document.activeElement as HTMLElement | null)?.blur(); incall.kbdOpen = false;
        await wait(50); nav.setTab('chats'); break;
      case 'from-calls':
        if (call.phase !== 'idle') call.hangup();
        nav.setTab('calls'); await wait(250);
        call.place(who); await wait(900); call.remote('answer'); break;
      case 'from-chat':
        if (call.phase !== 'idle') call.hangup();
        nav.setTab('chats'); nav.push({ name: 'conversation', chat: who }); await wait(350);
        call.place(who); await wait(900); call.remote('answer'); break;
      case 'incoming':
        if (call.phase !== 'idle') call.hangup();
        nav.setTab('contacts'); await wait(250);
        call.receive(who); await wait(200); call.restore(); await wait(900); call.answer(); break;
      case 'back': if (call.view === 'full') incall.goBack(); break;
      case 'message':
        // From the strip: back to the call screen first, then the button (the lab plays it as a person would).
        if (call.phase === 'idle') break;
        if (call.view !== 'full') { call.restore(); await wait(700); }
        if (incall.msg) incall.message(); break;
      case 'other':
        // Someone else's chat while the call runs (the green header button is not here: only the strip leads back).
        if (call.view === 'full') call.minimize();
        nav.setTab('chats'); nav.push({ name: 'conversation', chat: 'sana' }); break;
      case 'type': document.querySelector<HTMLTextAreaElement>('.composer textarea')?.focus(); break;
      case 'done': (document.activeElement as HTMLElement | null)?.blur(); incall.kbdOpen = false; break;
      case 'return': call.restore(); break;
    }
  });
}
