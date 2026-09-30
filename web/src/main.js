import './app.css';
import { mount } from 'svelte';
import { streamsAvailable } from './lib/stream.js';
import { ready } from './lib/crypto.js';
import { loadLanguage } from './lib/i18n.svelte.js';
import App from './App.svelte';

// The page's language first, so nothing shows in English and then switches.
await Promise.all([ready, loadLanguage()]);
// Start the service worker now: besides streaming, it tells people when
// the app this server sends changes (see public/sw.js).
streamsAvailable();
mount(App, { target: document.getElementById('app') });
