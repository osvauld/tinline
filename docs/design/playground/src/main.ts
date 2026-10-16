import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { applyMotionVars } from './lib/fx/motion';
import { startAudio } from './lib/fx/audio';
import { gate, type Step } from './lib/model/actions/gate.svelte';

applyMotionVars();
startAudio();

// `?lab=dark` shows the dark-mode depth lab instead of the app (design exploration, not to port).
const lab = new URLSearchParams(location.search).get('lab');
// `?lab=pair` shows two linked windows (Maya and Tara) to follow one contact addition on both sides; each window is
// the app with `?me=maya|tara`, which starts the link (lib/sim/pair.svelte.ts).
const labs: Record<string, () => Promise<{ default: any }>> = {
  dark: () => import('./screens/lab/DarkLab.svelte'),
  pair: () => import('./screens/lab/PairLab.svelte'),
  group: () => import('./screens/lab/GroupLab.svelte'),
  incall: () => import('./screens/lab/InCallLab.svelte'),
  photos: () => import('./screens/lab/PhotoLab.svelte'),
  // `?lab=voice`: the call's cloud field driven by the live mic, Fog and Clouds side by side.
  voice: () => import('./screens/lab/VoiceLab.svelte'),
  // `?lab=accounts`: switching identity on the login (unlock) screen, phone and desktop, three treatments.
  accounts: () => import('./screens/lab/AccountsLab.svelte'),
  // `?lab=avatars`: one-nickname initials and avatars drawn from the identity's fingerprint.
  avatars: () => import('./screens/lab/AvatarLab.svelte'),
  // `?lab=endcall`: the End button's glyph drawn from the logo, side by side with today's (lib/calls/EndGlyph.svelte).
  endcall: () => import('./screens/lab/EndCallLab.svelte'),
};
const app = lab && labs[lab]
  ? mount((await labs[lab]()).default, { target: document.getElementById('app')! })
  : mount(App, { target: document.getElementById('app')! });
const me = new URLSearchParams(location.search).get('me');
if (me === 'maya' || me === 'tara') (await import('./lib/sim/pair.svelte')).startPair(me);
// `?lab=group`: three windows in one group call, each the app as `?gme=maya|arjun|sam` (lib/sim/group.svelte.ts).
const gme = new URLSearchParams(location.search).get('gme');
if (gme === 'maya' || gme === 'arjun' || gme === 'sam') (await import('./lib/sim/group.svelte')).startGroupLab(gme);
// `?lab=incall`: three phones, each with a choice for messaging during a call (lib/calls/incall.svelte.ts).
if (new URLSearchParams(location.search).has('iclab')) (await import('./lib/calls/incall.svelte')).startInCallLab();
// `?step=<gate step>` opens the sign-up/unlock gate at that step (e.g. ?form=desktop&step=welcome).
const step = new URLSearchParams(location.search).get('step');
if (step) gate.jump(step as Step);
export default app;
