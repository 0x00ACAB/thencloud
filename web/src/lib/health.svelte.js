// The Health module: measurements and moods, kept in the encrypted "health"
// app data. Values are stored in metric units (kg, cm) and shown in the
// units chosen under Language and region (`unitSystem`); nothing leaves the
// browser unencrypted. `unloadHealth` drops it all when the signed-in view
// goes away.
import { loadAppData, saveAppData } from './cloud.svelte.js';
import { unitSystem, formatNumber } from './locale.svelte.js';
import { t } from './i18n.svelte.js';

const KG_PER_LB = 0.45359237;
const CM_PER_IN = 2.54;

/**
 * What can be logged. `dim` picks the unit setting that applies (mass or
 * length); `pair` means two values (blood pressure: systolic, diastolic).
 */
export const KINDS = {
  weight: { icon: 'scale', dim: 'mass', min: 1, max: 700, step: 0.1, get label() { return t('Weight'); } },
  height: { icon: 'ruler', dim: 'length', min: 30, max: 280, step: 0.5, get label() { return t('Height'); } },
  waist: { icon: 'ruler', dim: 'length', min: 20, max: 300, step: 0.5, get label() { return t('Waist'); } },
  fat: { icon: 'activity', unit: '%', min: 1, max: 80, step: 0.1, get label() { return t('Body fat'); } },
  pulse: {
    icon: 'heart-pulse',
    unit: 'bpm',
    min: 20,
    max: 250,
    step: 1,
    get label() { return t('Resting heart rate'); },
    get short() { return t('Heart rate'); },
  },
  pressure: {
    icon: 'activity',
    unit: 'mmHg',
    min: 30,
    max: 300,
    step: 1,
    get label() { return t('Blood pressure'); },
    get pair() { return [t('Systolic'), t('Diastolic')]; },
  },
  sleep: { icon: 'moon', unit: 'h', min: 0, max: 24, step: 0.25, get label() { return t('Sleep'); } },
};

export const health = $state({
  loaded: false,
  /** [{ id, kind, at (ms), value, value2?, note? }], metric units, oldest first. */
  measures: [],
  /** [{ id, at, x, y, word, note? }], x and y from -1 to 1, oldest first. */
  moods: [],
});

const finite = (v) => typeof v === 'number' && Number.isFinite(v);
const byTime = (a, b) => a.at - b.at;

function apply(d) {
  health.measures = (d.measures ?? []).filter((m) => KINDS[m.kind] && finite(m.value) && finite(m.at)).sort(byTime);
  health.moods = (d.moods ?? []).filter((m) => finite(m.x) && finite(m.y) && finite(m.at)).sort(byTime);
}

let loading = null;

export function loadHealth() {
  loading ??= loadAppData('health').then(
    (d) => {
      apply(d);
      health.loaded = true;
    },
    (e) => {
      loading = null;
      throw e;
    },
  );
  return loading;
}

export function unloadHealth() {
  loading = null;
  Object.assign(health, { loaded: false, measures: [], moods: [] });
}

async function save(change) {
  apply(await saveAppData('health', change));
}

const newId = () => crypto.randomUUID();

/** Log a measurement, with its values in display units. */
export function addMeasure({ kind, at, value, value2 = null, note = '' }) {
  const m = { id: newId(), kind, at, value: fromDisplay(kind, value) };
  if (KINDS[kind].pair) m.value2 = value2;
  if (note.trim()) m.note = note.trim();
  return save((d) => (d.measures = [...(d.measures ?? []), m]));
}

export const removeMeasure = (id) => save((d) => (d.measures = (d.measures ?? []).filter((m) => m.id !== id)));

export function addMood({ at, x, y, word, note = '' }) {
  const r = (v) => Math.round(v * 100) / 100;
  const m = { id: newId(), at, x: r(x), y: r(y), word };
  if (note.trim()) m.note = note.trim();
  return save((d) => (d.moods = [...(d.moods ?? []), m]));
}

export const removeMood = (id) => save((d) => (d.moods = (d.moods ?? []).filter((m) => m.id !== id)));

/** Put a removed entry back (undo). */
export const putBack = (list, item) => save((d) => (d[list] = [...(d[list] ?? []).filter((x) => x.id !== item.id), item]));


// ---------------------------------------------------------------- units

/** The unit a kind is shown in. */
export function unitOf(kind) {
  const k = KINDS[kind];
  const imperial = unitSystem() === 'imperial';
  if (k.dim === 'mass') return imperial ? 'lb' : 'kg';
  if (k.dim === 'length') return imperial ? 'in' : 'cm';
  return k.unit;
}

/** Stored (metric) to shown. */
export function toDisplay(kind, v) {
  const u = unitOf(kind);
  return u === 'lb' ? v / KG_PER_LB : u === 'in' ? v / CM_PER_IN : v;
}

/** Shown to stored (metric). */
export function fromDisplay(kind, v) {
  const u = unitOf(kind);
  return u === 'lb' ? v * KG_PER_LB : u === 'in' ? v * CM_PER_IN : v;
}

/** "72.4 kg", "120/80 mmHg". */
export function formatMeasure(m) {
  const k = KINDS[m.kind];
  const n = (v) => formatNumber(Math.round(v * 10) / 10, { maximumFractionDigits: 1 });
  if (k.pair) return `${Math.round(m.value)}/${Math.round(m.value2)} ${k.unit}`;
  return `${n(toDisplay(m.kind, m.value))} ${unitOf(m.kind)}`;
}

/** Body mass index from kg and cm, or null. */
export const bmi = (kg, cm) => (kg > 0 && cm > 0 ? kg / (cm / 100) ** 2 : null);

/** The rough band a BMI falls in (the WHO's adult ranges). */
export function bmiBand(v) {
  if (v < 18.5) return t('Underweight');
  if (v < 25) return t('Healthy range');
  if (v < 30) return t('Overweight');
  return t('Obese range');
}
