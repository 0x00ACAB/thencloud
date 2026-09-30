import './app.css';
import { mount } from 'svelte';
import { streamsAvailable } from './lib/stream.js';
import { ready } from './lib/crypto.js';
import { loadLanguage, language, t } from './lib/i18n.svelte.js';
import SharePage from './SharePage.svelte';

// The page's language first, so nothing shows in English and then switches.
await Promise.all([ready, loadLanguage()]);
document.documentElement.lang = language();
document.title = `${t('Shared with you')} · thencloud`;
// Start the service worker now: besides streaming, it tells people when
// the app this server sends changes (see public/sw.js).
streamsAvailable();
mount(SharePage, { target: document.getElementById('app') });
