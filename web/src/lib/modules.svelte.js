// Optional parts of the app, off until turned on in Settings. Which ones are
// on is kept in the encrypted "prefs" app data, so it follows the account to
// every device and the server doesn't learn it.
import { loadAppData, saveAppData } from './cloud.svelte.js';

export const MODULES = [
  {
    name: 'health',
    label: 'Health',
    icon: 'heart-pulse',
    description: 'A log of weight, height, blood pressure and other measurements with charts, and a mood meter. Encrypted like your files.',
  },
];

/** name -> on. */
export const modules = $state({ health: false, loaded: false });

let loading = null;

export function loadModules() {
  loading ??= loadAppData('prefs').then(
    (d) => {
      for (const m of MODULES) modules[m.name] = !!d.modules?.[m.name];
      modules.loaded = true;
    },
    (e) => {
      loading = null;
      throw e;
    },
  );
  return loading;
}

/** Turn a module on or off. Its data stays either way. */
export async function setModule(name, on) {
  modules[name] = on;
  try {
    await saveAppData('prefs', (d) => {
      d.modules ??= {};
      d.modules[name] = on;
    });
  } catch (e) {
    modules[name] = !on;
    throw e;
  }
}
