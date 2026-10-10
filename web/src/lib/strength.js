// Password strength, estimated in the browser with zxcvbn (the estimator
// KeePassXC uses). Its word lists are large, so they're loaded with import()
// only when a password is being chosen.

let factory;

function load() {
  factory ??= Promise.all([import('@zxcvbn-ts/core'), import('@zxcvbn-ts/language-common'), import('@zxcvbn-ts/language-en')]).then(
    ([core, common, en]) =>
      new core.ZxcvbnFactory({
        dictionary: { ...common.dictionary, ...en.dictionary },
        graphs: common.adjacencyGraphs,
      }),
  );
  return factory;
}

/** 0 (guessable in moments) to 4 (very hard); `inputs` are words the password shouldn't lean on, like the username. */
export async function passwordScore(password, inputs = []) {
  const zxcvbn = await load();
  return zxcvbn.check(password, inputs.filter(Boolean)).score;
}
