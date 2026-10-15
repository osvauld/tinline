/* This device's own settings (not synced, not part of an account): video quality, camera, downloads, storage use.
   Simulated values; the Simulation drawer can change the camera list. */
export type AutoDl = 'always' | 'wifi' | 'never';
export interface Camera { id: string; label: string; usb?: boolean }

class Device {
  /** Prefer HD: aim for 720p (phone 540p), still adapting to the connection. Off = Auto. Applies at the next camera start. */
  hd = $state(false);
  cameras = $state<Camera[]>([
    { id: 'builtin', label: 'Built-in camera' },
    { id: 'c920', label: 'Logitech C920', usb: true },
  ]);
  camera = $state('builtin');
  /** Pictures that arrive start downloading by themselves (over the limit they wait for a tap). */
  autoDl = $state<AutoDl>('wifi');
  /** Bytes used, by kind (simulated). */
  used = $state({ pictures: 412e6, files: 96e6, chats: 18e6, cache: 57e6 });
  clearCache() { this.used.cache = 0; }
}
export const device = new Device();

export const fmtBytes = (n: number) => n >= 1e9 ? (n / 1e9).toFixed(1) + ' GB' : n >= 1e6 ? Math.round(n / 1e6) + ' MB' : n >= 1e3 ? Math.round(n / 1e3) + ' KB' : n + ' B';
