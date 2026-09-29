// The mood meter's words in the current language. Moods are stored with the
// English word (see mood.js), so what's logged doesn't depend on the
// language it was logged in; this is only for showing them.
import { t } from './i18n.svelte.js';

const WORD = {
  Furious: () => t('Furious'),
  Panicked: () => t('Panicked'),
  Stressed: () => t('Stressed'),
  Surprised: () => t('Surprised'),
  Thrilled: () => t('Thrilled'),
  Ecstatic: () => t('Ecstatic'),
  Angry: () => t('Angry'),
  Anxious: () => t('Anxious'),
  Restless: () => t('Restless'),
  Energised: () => t('Energised'),
  Cheerful: () => t('Cheerful'),
  Inspired: () => t('Inspired'),
  Irritated: () => t('Irritated'),
  Worried: () => t('Worried'),
  Uneasy: () => t('Uneasy'),
  Pleasant: () => t('Pleasant'),
  Hopeful: () => t('Hopeful'),
  Proud: () => t('Proud'),
  Sad: () => t('Sad'),
  Glum: () => t('Glum'),
  Bored: () => t('Bored'),
  'At ease': () => t('At ease'),
  Content: () => t('Content'),
  Grateful: () => t('Grateful'),
  Lonely: () => t('Lonely'),
  Drained: () => t('Drained'),
  Tired: () => t('Tired'),
  Calm: () => t('Calm'),
  Relaxed: () => t('Relaxed'),
  Fulfilled: () => t('Fulfilled'),
  Hopeless: () => t('Hopeless'),
  Despairing: () => t('Despairing'),
  Exhausted: () => t('Exhausted'),
  Sleepy: () => t('Sleepy'),
  Peaceful: () => t('Peaceful'),
  Serene: () => t('Serene'),
};

/** A mood word as shown; one we don't know is shown as stored. */
export const moodWord = (word) => WORD[word]?.() ?? word;
