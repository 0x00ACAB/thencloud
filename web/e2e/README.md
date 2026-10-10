# Browser tests

Playwright drives the built web client against a real server, with an empty
data directory each run, and fails if any request carries a file's name or
contents, a password or a link key.

Build first, from the repository root:

```sh
./build.sh
```

With Playwright's Chromium installed (`npx playwright install chromium`):

```sh
cd web && npx playwright test
```

Without a browser on the machine, start the server yourself and run the
tests in the Playwright image, which reuses it (keep the image's version the
same as `@playwright/test` in package.json):

```sh
rm -rf target/e2e-data
node web/e2e/fake-google.mjs 8094 &
target/debug/thencloud-server --bind 127.0.0.1:8093 --data-dir target/e2e-data --web-dir web/dist \
  --google-client-id e2e --google-client-secret e2e --google-test-base http://127.0.0.1:8094 &
docker run --rm --network host --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD/web:/work" -w /work \
  mcr.microsoft.com/playwright:v1.63.0-noble \
  npx playwright test
```

The first account on a new server needs its setup code: `signUp` reads it from `../target/e2e-data/setup-code`. Against a server that's already running (`THENCLOUD_E2E_URL`), pass it in `THENCLOUD_E2E_SETUP_CODE` if that server has no accounts yet.

Linking Google Drive goes to `e2e/fake-google.mjs`, which playwright.config.js starts on port 8094 (`THENCLOUD_E2E_GOOGLE_PORT`) and the server reaches through the hidden `--google-test-base` flag. Its `/auth` sends the popup straight back, as if access was allowed; `GET /_files` lists what was stored there, so tests can check it's only ciphertext.
