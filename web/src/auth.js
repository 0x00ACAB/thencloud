import './app.css';
import { mount } from 'svelte';
import { streamsAvailable } from './lib/stream.js';
import { loadLanguage, language } from './lib/i18n.svelte.js';
import AuthCheck from './AuthCheck.svelte';

// No crypto here: this page never sees a password or a key.
await loadLanguage();
document.documentElement.lang = language();
// Start the service worker now: besides streaming, it tells people when
// the app this server sends changes (see public/sw.js).
streamsAvailable();
mount(AuthCheck, { target: document.getElementById('app') });
