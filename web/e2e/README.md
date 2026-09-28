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
target/debug/thencloud-server --bind 127.0.0.1:8093 --data-dir target/e2e-data --web-dir web/dist &
docker run --rm --network host --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD/web:/work" -w /work \
  mcr.microsoft.com/playwright:v1.63.0-noble \
  npx playwright test
```
