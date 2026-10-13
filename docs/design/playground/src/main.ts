import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { applyMotionVars } from './lib/fx/motion';
import { startAudio } from './lib/fx/audio';

applyMotionVars();
startAudio();

export default mount(App, { target: document.getElementById('app')! });
