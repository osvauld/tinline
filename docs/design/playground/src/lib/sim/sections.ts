/* Simulation drawer sections. Each area registers ONE component here (append a line);
   the drawer renders them in order under their titles. Sim controls never appear inside the app frame. */
import type { Component } from 'svelte';
import CoreSim from './CoreSim.svelte';
import ChatSim from './ChatSim.svelte';
import GateSim from './GateSim.svelte';
import CallsSim from './CallsSim.svelte';

export const simSections: { title: string; component: Component }[] = [
  { title: 'Environment', component: CoreSim },
  { title: 'Chat', component: ChatSim },
  { title: 'Sign-up and unlock', component: GateSim },
  { title: 'Calls', component: CallsSim },
];
