// The mood meter: how pleasant a feeling is (left to right) against how much
// energy is in it (bottom to top), as a 6 x 6 grid of words. Each corner has
// its colour: red (unpleasant, lots of energy), yellow (pleasant, lots),
// blue (unpleasant, little), green (pleasant, little).

export const SIZE = 6;

/** Rows from most energy to least; columns from least pleasant to most. */
export const WORDS = [
  ['Furious', 'Panicked', 'Stressed', 'Surprised', 'Thrilled', 'Ecstatic'],
  ['Angry', 'Anxious', 'Restless', 'Energised', 'Cheerful', 'Inspired'],
  ['Irritated', 'Worried', 'Uneasy', 'Pleasant', 'Hopeful', 'Proud'],
  ['Sad', 'Glum', 'Bored', 'At ease', 'Content', 'Grateful'],
  ['Lonely', 'Drained', 'Tired', 'Calm', 'Relaxed', 'Fulfilled'],
  ['Hopeless', 'Despairing', 'Exhausted', 'Sleepy', 'Peaceful', 'Serene'],
];

/** The centre of a cell as (x, y), each from -1 to 1 (y up). */
export function cellPoint(row, col) {
  const step = 2 / SIZE;
  return { x: -1 + step * (col + 0.5), y: 1 - step * (row + 0.5) };
}

/** The cell a point falls in. */
export function pointCell(x, y) {
  const clamp = (v) => (Number.isFinite(v) ? Math.max(0, Math.min(SIZE - 1, v)) : SIZE / 2);
  return { row: clamp(Math.floor(((1 - y) / 2) * SIZE)), col: clamp(Math.floor(((x + 1) / 2) * SIZE)) };
}

/** red, yellow, blue or green. */
export function quadrant(x, y) {
  if (y >= 0) return x < 0 ? 'red' : 'yellow';
  return x < 0 ? 'blue' : 'green';
}

/** How far into its corner a point is, 0 (middle) to 1. */
export const strength = (x, y) => Math.min(1, Math.max(Math.abs(x), Math.abs(y)));

/** The word for a point. */
export function wordAt(x, y) {
  const { row, col } = pointCell(x, y);
  return WORDS[row][col];
}
