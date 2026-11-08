import '@fontsource/anton/400.css';
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-sans/600.css';
import '@fontsource/ibm-plex-sans/700.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import '@fontsource/ibm-plex-mono/600.css';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { ready } from './lib/host';

await ready;

export default mount(App, { target: document.getElementById('app')! });
