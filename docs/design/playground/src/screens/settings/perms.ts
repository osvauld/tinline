/* The OS-permission problems the app can be in, with calm copy for each. One list, used by the Settings landing strip,
   the Notifications page and the banners, so the wording is the same everywhere. */
import { pp } from '../../lib/model/actions/people.svelte';
import { prefs } from '../../lib/model/prefs.svelte';

export interface Problem { key: 'notifications' | 'microphone' | 'battery'; title: string; body: string; fix: string; icon: string }

export function problems(): Problem[] {
  const out: Problem[] = [];
  if (pp.perms.notifications === 'denied') out.push({ key: 'notifications', icon: 'bell', title: 'Notifications are off',
    body: 'Tinline can’t alert you to messages or calls. They still arrive; you’ll see them when you open the app. Turn notifications on in system settings.', fix: 'Open settings' });
  if (pp.perms.microphone === 'denied') out.push({ key: 'microphone', icon: 'micoff', title: 'Microphone is off',
    body: 'You can take and place calls, but people won’t hear you until Tinline can use the microphone. Turn it on in system settings.', fix: 'Open settings' });
  if (prefs.form === 'android' && pp.perms.battery === 'restricted') out.push({ key: 'battery', icon: 'battery', title: 'Battery saving may delay calls',
    body: 'Android can pause Tinline in the background, so calls and messages can arrive late. Set Tinline’s battery use to Unrestricted.', fix: 'Open settings' });
  return out;
}

/** Fixing happens in the system's own settings, outside the app. The playground treats "Open settings" as the person
    changing it there and coming back. */
export function fix(key: Problem['key']) {
  if (key === 'battery') pp.perms.battery = 'ok'; else pp.perms[key] = 'granted';
}
