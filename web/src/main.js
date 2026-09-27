import './app.css';
import { mount } from 'svelte';
import { ready } from './lib/crypto.js';
import App from './App.svelte';

await ready;
mount(App, { target: document.getElementById('app') });
